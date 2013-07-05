//
//  PutioBrowser.m
//  put.io adder
//
//  Created by Nico on 7/4/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import "PutioAppDelegate.h"
#import "PutioBrowser.h"
#import "SSKeychain.h"

@implementation PutioBrowser

@synthesize webView;


-(void)awakeFromNib
{
    [[webView mainFrame] loadRequest:[NSURLRequest requestWithURL:[NSURL URLWithString:@"https://api.put.io/v2/oauth2/authenticate?client_id=711&response_type=token&redirect_uri=http://nicoswd.com/sites/putio/"]]];
}


-(void)webView:(WebView *)sender didFinishLoadForFrame:(WebFrame *)frame
{    
    NSString *searchedString = [webView.mainFrame.dataSource.request.URL absoluteString];
    NSError* error = nil;
    
    NSRegularExpression* regex = [NSRegularExpression regularExpressionWithPattern:@"#access_token=([A-Za-z0-9]+)" options:0 error:&error];
    NSArray* matches = [regex matchesInString:searchedString options:0 range:NSMakeRange(0, [searchedString length])];
    
    if ([matches count] > 0)
    {
        NSString *token = [searchedString substringWithRange:[[matches objectAtIndex:0] rangeAtIndex:1]];
    
        bool res = [SSKeychain setPassword:token forService:@"put.io adder" account:@"711"];
        PutioAppDelegate *del = [[NSApplication sharedApplication] delegate];

        if (res == YES)
        {
            del.message.stringValue = @"Authenticated and ready to go!";
            [self.window close];
        }
        else
        {
            del.message.stringValue = @"Error saving token to KeyChain!";
        }
    }
}

@end