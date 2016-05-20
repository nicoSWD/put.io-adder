//
//  PutioResultsViewController.h
//  put.io adder
//
//  Created by Nico on 05/11/14.
//  Copyright (c) 2014 Nicolas Oelgart. All rights reserved.
//

#import <Cocoa/Cocoa.h>

@interface PutioResultsViewController : NSViewController <NSTableViewDataSource, NSTableViewDelegate>
{
    NSMutableArray *files;
    IBOutlet NSTableView *tableView;
}

@property (strong) NSMutableArray *files;
@property (strong, nonatomic) IBOutlet NSTableView *tableView;

@end