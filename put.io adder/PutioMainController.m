//
//  PutioMainController.m
//  put.io adder
//
//  Created by Nico on 7/5/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import "PutioMainController.h"
#import "AFNetworking.h"
#import "AFJSONRequestOperation.h"
#import "PutioBrowser.h"
#import "SSKeychain.h"

@implementation PutioMainController

@synthesize message, progress, waiting, authWindow, oauthToken, waitingLabel, userInfo;


-(void)awakeFromNib
{
    [[NSAppleEventManager sharedAppleEventManager] setEventHandler:self andSelector:@selector(handleEvent:withReplyEvent:) forEventClass:kInternetEventClass andEventID:kAEGetURL];
        
    [self.waiting startAnimation:nil];
    [self.progress stopAnimation:nil];
    [self authenticateUser];
}


- (void)authenticateUser
{
    NSError *error;
    NSString *t = [SSKeychain passwordForService:@"put.io adder" account:@"711" error:&error];
    
    if ([error code] == SSKeychainErrorNotFound)
    {
        self.message.stringValue = @"Authentication required!";
        [self.waitingLabel setHidden:YES];
        [self.waiting stopAnimation:nil];
        
        self.authWindow = [[PutioBrowser alloc] initWithWindowNibName:@"Browser"];
        [self.authWindow.window makeKeyWindow];
        [self.authWindow.window makeMainWindow];
        
        /////////////////////
        // TODO: move auth window in front of *ALL* other windows.
        // Seems simple enough, but I can't get it to work.
        //
        // Pull request, anyone?
        /////////////////////
    }
    else
    {
        self.oauthToken = t;
        [self updateUserInfo];
        
        userInfoTimer = [NSTimer scheduledTimerWithTimeInterval:60.0  target:self selector:@selector(updateUserInfo) userInfo:nil repeats:YES];
    }
}


- (void)updateUserInfo
{
    if (self.oauthToken == nil)
    {
        return;
    }

    NSURL *url = [NSURL URLWithString:[NSString stringWithFormat:@"https://api.put.io/v2/account/info?oauth_token=%@", self.oauthToken]];
    NSURLRequest *request = [NSURLRequest requestWithURL:url];
    AFJSONRequestOperation *operation = [AFJSONRequestOperation JSONRequestOperationWithRequest:request
    success:^(NSURLRequest *request, NSHTTPURLResponse *response, id JSON)
    {
        if (![JSON count])
        {
            return;
        }

        NSMutableArray *_userInfo = [JSON objectForKey:@"info"];
        NSMutableArray *disk = [_userInfo valueForKey:@"disk"];
        
        self.userInfo.stringValue = [NSString stringWithFormat:@"%@, %@ used of %@",
            [_userInfo valueForKey:@"username"],
            [self transformedValue:[disk valueForKey:@"used"]],
            [self transformedValue:[disk valueForKey:@"size"]]
        ];
    }
    failure:^(NSURLRequest *request, NSURLResponse *response, NSError *error, id JSON)
    {
        self.userInfo.stringValue = @"Failed to fetch account info!";
    }];
    
    [operation start];
}


/**
 * transformedValue method by "Parag Bafna", from: http://stackoverflow.com/questions/7846495/
 *
**/
- (id)transformedValue:(id)value
{
    double convertedValue = [value doubleValue];
    int multiplyFactor = 0;
    
    NSArray *tokens = [NSArray arrayWithObjects:@"bytes",@"KiB",@"MiB",@"GiB",@"TiB",nil];
    
    while (convertedValue > 1024)
    {
        convertedValue /= 1024;
        multiplyFactor++;
    }
    
    return [NSString stringWithFormat:@"%4.2f %@", convertedValue, [tokens objectAtIndex:multiplyFactor]];
}


- (void)handleEvent:(NSAppleEventDescriptor *)event withReplyEvent:(NSAppleEventDescriptor *)replyEvent
{
    if (self.oauthToken == nil)
    {
        return [self authenticateUser];
    }
    
    [self.waiting stopAnimation:nil];
    [self.progress startAnimation:nil];
    [self.waitingLabel setHidden:YES];
    
    NSString *magnetURL = [[event paramDescriptorForKeyword:keyDirectObject] stringValue];
    NSError *error = nil;
    
    NSRegularExpression* regex = [NSRegularExpression regularExpressionWithPattern:@"dn=([^&]{5,})" options:0 error:&error];
    NSArray* matches = [regex matchesInString:magnetURL options:0 range:NSMakeRange(0, [magnetURL length])];
    NSString *displayName;
    
    if ([matches count] > 0)
    {
        displayName = [magnetURL substringWithRange:[[matches objectAtIndex:0] rangeAtIndex:1]];
        displayName = [displayName stringByReplacingOccurrencesOfString:@"+" withString:@" "];
        displayName = [displayName stringByReplacingOccurrencesOfString:@"." withString:@" "];
        displayName = [displayName stringByReplacingPercentEscapesUsingEncoding:NSUTF8StringEncoding];
    }
    else
    {
        displayName = magnetURL;
    }
    
    self.message.stringValue = [NSString stringWithFormat:@"Adding: %@", displayName];
    
    NSURL *url = [NSURL URLWithString:@"https://api.put.io/"];
    NSDictionary *params = [NSDictionary dictionaryWithObjectsAndKeys:
        magnetURL, @"url",
        self.oauthToken, @"oauth_token",
    nil];
    
    AFHTTPClient *httpClient = [[AFHTTPClient alloc] initWithBaseURL:url];
    NSMutableURLRequest *request = [httpClient requestWithMethod:@"POST" path:@"/v2/transfers/add" parameters:params];
    AFJSONRequestOperation *operation = [AFJSONRequestOperation JSONRequestOperationWithRequest:request
    success:^(NSURLRequest *request, NSHTTPURLResponse *response, id JSON)
    {
        if ([[JSON valueForKeyPath:@"status"] isEqualToString:@"OK"])
        {
            self.message.stringValue = @"URL successfully added!";
        }
        else
        {
            self.message.stringValue = @"Something went wrong!";
        }

        [self.waiting startAnimation:nil];
        [self.progress stopAnimation:nil];
        [self.waitingLabel setHidden:NO];
    }
    failure:^(NSURLRequest *request, NSHTTPURLResponse *response, NSError *error, id JSON)
    {
        long statusCode = (long)response.statusCode;

        if (statusCode == 400)
        {
            self.message.stringValue = @"Failed! URL already in queue?";
        }
        else
        {
            self.message.stringValue = [NSString stringWithFormat:@"Something went wrong! HTTP error %li!", statusCode];
        }

        [self.waiting startAnimation:nil];
        [self.progress stopAnimation:nil];
        [self.waitingLabel setHidden:NO];
    }];
    
    [operation start];
}


- (IBAction)loadWebsite:(id)sender
{
	[[NSWorkspace sharedWorkspace] openURL:[NSURL URLWithString:@"https://github.com/nicoSWD/put.io-adder"]];
}

@end