//
//  PutioBrowser.m
//  put.io adder
//
//  Created by Nico on 7/4/13.
//  Copyright (c) 2013 Nicolas Oelgart. All rights reserved.
//

#import "PutioMainController.h"
#import "PutioBrowser.h"
#import "SSKeychain.h"

@implementation PutioBrowser

@synthesize webView;


-(void)awakeFromNib
{
    NSURL *url = [NSURL URLWithString:@"https://api.put.io/v2/oauth2/authenticate?client_id=711&response_type=token&redirect_uri=http://nicoswd.com/sites/putio/"];
    [[webView mainFrame] loadRequest:[NSURLRequest requestWithURL:url]];
}


-(void)webView:(WebView *)sender didFinishLoadForFrame:(WebFrame *)frame
{
    NSString *searchedString = [webView.mainFrame.dataSource.request.URL absoluteString];
    NSError *error = nil;
    
    if (searchedString == nil)
    {
        return;
    }
    
    NSRegularExpression* regex = [NSRegularExpression regularExpressionWithPattern:@"#access_token=([A-Za-z0-9]+)" options:0 error:&error];
    NSArray* matches = [regex matchesInString:searchedString options:0 range:NSMakeRange(0, [searchedString length])];
    
    if ([matches count] > 0)
    {
        PutioMainController *controller = [[[[NSApplication sharedApplication] windows] objectAtIndex:0] windowController];
        
        NSString *token = [searchedString substringWithRange:[[matches objectAtIndex:0] rangeAtIndex:1]];
    
        if ([SSKeychain setPassword:token forService:@"put.io adder" account:@"711"])
        {
            controller.message.stringValue = @"Authenticated and ready to go!";
            [controller.waiting startAnimation:nil];
            [controller.waitingLabel setHidden:NO];
            
            [self.window close];
        }
        else
        {
            controller.message.stringValue = @"Error saving token to KeyChain!";
        }
    }
}

@end